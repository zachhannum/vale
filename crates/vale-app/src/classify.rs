//! Classes of the values of a field or an expression, and a rule for each
//! class. Numbers have three methods. Text gives a class for each common
//! value and one class for the other values.

use std::collections::HashMap;

use vale_store::{FieldType, GeometryKind, Layer, Value};
use vale_style::expr::{Context, Expr, Filter, Property, field_ref};
use vale_style::{Color, Rule, Stroke};

use crate::ramps::Ramp;
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
/// The field that keeps the color of each polygon when neighbors differ.
pub const COLOR_FIELD: &str = "color_group";
/// The smallest and the largest number of colors when neighbors differ.
pub const COLORS: (usize, usize) = (4, 12);
pub const DEFAULT_COLORS: usize = 5;
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
    for (j, b) in best[0].iter_mut().enumerate().skip(1) {
        *b = (cost(0, j), 0);
    }
    for c in 1..n {
        let (done, rest) = best.split_at_mut(c);
        let (prev, row) = (&done[c - 1], &mut rest[0]);
        for (j, b) in row.iter_mut().enumerate().skip(c + 1) {
            *b = (f64::INFINITY, c);
            for (i, p) in prev.iter().enumerate().take(j).skip(c) {
                let total = p.0 + cost(i, j);
                if total < b.0 {
                    *b = (total, i);
                }
            }
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

/// The values that are not null or blank, each one time, most common first,
/// then in text order.
pub fn categories(values: &[Value]) -> Vec<Value> {
    let mut counts: HashMap<String, (usize, &Value)> = HashMap::new();
    for v in values {
        let Some(key) = value_text(v) else {
            continue;
        };
        counts.entry(key).or_insert((0, v)).0 += 1;
    }
    let mut list: Vec<(String, usize, &Value)> =
        counts.into_iter().map(|(k, (n, v))| (k, n, v)).collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    list.into_iter().map(|(_, _, v)| v.clone()).collect()
}

/// The values of `categories` that get a class: each value that is a label
/// of the ramp, and the `max` most common other values.
pub fn common(categories: &[Value], max: usize, ramp: Option<&Ramp>) -> Vec<Value> {
    let mut rest = max;
    categories
        .iter()
        .filter(|v| {
            let labeled = ramp
                .zip(value_text(v))
                .is_some_and(|(r, t)| r.labeled(&t).is_some());
            if labeled {
                return true;
            }
            let keep = rest > 0;
            rest = rest.saturating_sub(1);
            keep
        })
        .cloned()
        .collect()
}

/// A value as the text that a label matches. `None` for null and blank text.
fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Text(t) if t.trim().is_empty() => None,
        Value::Text(t) => Some(t.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(number_text(*n)),
    }
}

/// A class: the name and the filter of its rule. The rule of an extra
/// class takes the values that the other classes do not take.
#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub name: String,
    pub filter: String,
    pub extra: bool,
    /// The value of a class of a text value, as text.
    pub value: Option<String>,
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
            value: None,
        });
    }
    for (k, pair) in edges.windows(2).enumerate() {
        if k == 0 {
            out.push(Class {
                name: format!("< {}", number_text(pair[0])),
                filter: format!("{left} < {}", number_text(pair[0])),
                extra: false,
                value: None,
            });
        }
        let (a, b) = (number_text(pair[0]), number_text(pair[1]));
        out.push(Class {
            name: format!("{a} – {b}"),
            filter: format!("{left} >= {a} and {left} < {b}"),
            extra: false,
            value: None,
        });
    }
    if let [only] = edges {
        out.push(Class {
            name: format!("< {}", number_text(*only)),
            filter: format!("{left} < {}", number_text(*only)),
            extra: false,
            value: None,
        });
    }
    if let Some(last) = edges.last() {
        out.push(Class {
            name: format!("≥ {}", number_text(*last)),
            filter: format!("{left} >= {}", number_text(*last)),
            extra: false,
            value: None,
        });
    }
    if nulls {
        out.push(Class {
            name: NO_VALUE.to_owned(),
            filter: format!("{left} is null"),
            extra: true,
            value: None,
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
            value: value_text(v),
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
        value: None,
    });
    out
}

/// The field of a layer that takes the colors of `neighbors::color`:
/// `COLOR_FIELD` if the layer has no such field or if it is a field of whole
/// numbers, or else the first free name with a number after it.
pub fn color_field(layer: &Layer) -> String {
    (1..)
        .map(|i| match i {
            1 => COLOR_FIELD.to_owned(),
            _ => format!("{COLOR_FIELD}_{i}"),
        })
        .find(|name| match layer.fields.iter().find(|f| f.name == *name) {
            Some(f) => f.kind == FieldType::Integer,
            None => layer.name_free(name, None),
        })
        .unwrap_or_default()
}

/// The classes of `count` colors in the field `field`, with the values 1 to
/// `count`, and the class `OTHER_VALUES` for the features with no color.
pub fn color_classes(field: &str, count: usize) -> Vec<Class> {
    let values: Vec<Value> = (1..=count).map(|i| Value::Number(i as f64)).collect();
    let mut classes = category_classes(&field_ref(field), &values);
    for (i, class) in classes.iter_mut().filter(|c| !c.extra).enumerate() {
        class.name = format!("Color {}", i + 1);
        class.value = None;
    }
    classes
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
/// a layer, with `n` classes of numbers. Each text value that is a label of
/// `ramp` gets a class. `None` if the text is not an expression or if no
/// feature has a value.
pub fn classes_of(
    layer: &Layer,
    text: &str,
    method: Method,
    n: usize,
    ramp: Option<&Ramp>,
) -> Option<Vec<Class>> {
    let expr = Expr::parse(text).ok()?;
    let all = values(layer, &expr);
    let left = left_side(text);
    let classes = match numbers(&all) {
        Some((sorted, nulls)) => number_classes(&left, &breaks(&sorted, method, n), nulls > 0),
        None => {
            let common = common(&categories(&all), MAX_CATEGORIES, ramp);
            if common.is_empty() {
                return None;
            }
            category_classes(&left, &common)
        }
    };
    Some(classes)
}

/// The colors of the classes that are not extra, from a ramp that is in
/// reverse order with `reverse`. A class whose value is the label of a color
/// takes that color. If a class takes a label, each other class takes the
/// next color that no label took. If no class takes a label, the colors
/// come from `Ramp::classes`.
pub fn class_colors(ramp: &Ramp, classes: &[Class], reverse: bool) -> Vec<Color> {
    let mut ramp = ramp.clone();
    if reverse {
        ramp.reverse();
    }
    let open: Vec<&Class> = classes.iter().filter(|c| !c.extra).collect();
    let matched: Vec<Option<usize>> = open
        .iter()
        .map(|c| c.value.as_deref().and_then(|v| ramp.labeled(v)))
        .collect();
    let rgb = if matched.iter().all(Option::is_none) {
        ramp.classes(open.len())
    } else {
        let mut free: Vec<Rgb> = (0..ramp.colors.len())
            .filter(|i| !matched.contains(&Some(*i)))
            .map(|i| ramp.colors[i])
            .collect();
        if free.is_empty() {
            free = ramp.colors.clone();
        }
        let mut rest = free.iter().copied().cycle();
        matched
            .iter()
            .map(|m| match m {
                Some(i) => ramp.colors[*i],
                None => rest.next().unwrap_or_default(),
            })
            .collect()
    };
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
    fn color_classes_have_a_rule_for_each_color_and_one_for_the_others() {
        let classes = color_classes("color_group", 3);
        let names: Vec<&str> = classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Color 1", "Color 2", "Color 3", OTHER_VALUES]);
        assert_eq!(classes[1].filter, "color_group = 2");
        let attrs = |v: Value| std::collections::BTreeMap::from([("color_group".to_owned(), v)]);
        let other = Filter::new(&classes[3].filter);
        assert!(other.error().is_none());
        for (v, inside) in [
            (Value::Null, true),
            (Value::Number(4.0), true),
            (Value::Number(2.0), false),
        ] {
            let context = Context {
                attributes: &attrs(v.clone()),
                scale: 0.0,
                geometry: GeometryKind::Polygon,
            };
            assert_eq!(other.matches(&context), inside, "{v:?}");
        }
    }

    #[test]
    fn text_gives_a_class_for_each_value_and_one_for_the_others() {
        let values: Vec<Value> = ["lake", "forest", "lake", "", "it's"]
            .iter()
            .map(|t| Value::Text((*t).to_owned()))
            .chain([Value::Null])
            .collect();
        let common = common(&categories(&values), MAX_CATEGORIES, None);
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
    fn a_value_takes_the_color_of_its_label() {
        let mut ramp = Ramp::new("Zones", vec![[1; 3], [2; 3], [3; 3], [4; 3]]);
        ramp.labels[1] = "Cfb".into();
        ramp.labels[3] = "true".into();
        ramp.qualitative = true;
        let values = [
            Value::Text("Moon".into()),
            Value::Text("cfb".into()),
            Value::Text("Sun".into()),
        ];
        let classes = category_classes("kind", &values);
        let colors = class_colors(&ramp, &classes, false);
        let c = |v: u8| Color::rgb(v, v, v);
        // "Moon" and "Sun" take the colors that no label took, in order.
        assert_eq!(colors, [c(1), c(2), c(3)]);
        let reversed = class_colors(&ramp, &classes, true);
        assert_eq!(reversed, [c(4), c(2), c(3)]);
        // A bool value matches a label too.
        let flags = category_classes("wet", &[Value::Bool(true)]);
        assert_eq!(class_colors(&ramp, &flags, false), [c(4)]);
        // With no label that matches, the colors come in order.
        let plain = category_classes("kind", &values[..1]);
        assert_eq!(class_colors(&ramp, &plain, false), [c(1)]);
    }

    #[test]
    fn each_value_with_a_label_gets_a_class_past_the_most_common() {
        let ramps = crate::ramps::Ramps::default();
        let koppen = ramps
            .list()
            .iter()
            .find(|r| r.name == crate::ramps::KOPPEN)
            .unwrap();
        // Each code shows fewer times than the code before it, so "Dwb"
        // is not among the 12 most common values.
        let mut values = Vec::new();
        for (i, code) in koppen.labels.iter().enumerate() {
            for _ in 0..30 - i {
                values.push(Value::Text(code.clone()));
            }
        }
        values.push(Value::Text("Moon".into()));
        let sorted = categories(&values);
        assert_eq!(sorted.len(), 31);
        let plain = common(&sorted, MAX_CATEGORIES, None);
        assert_eq!(plain.len(), MAX_CATEGORIES);
        assert!(!plain.contains(&Value::Text("Dwb".into())));
        let labeled = common(&sorted, MAX_CATEGORIES, Some(koppen));
        assert_eq!(labeled.len(), 31);
        let classes = category_classes("climate", &labeled);
        let colors = class_colors(koppen, &classes, false);
        let dwb = classes.iter().position(|c| c.name == "Dwb").unwrap();
        assert_eq!(colors[dwb], Color::rgb(90, 120, 220));
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
