//! The functions of the expression language.

use vale_store::{Value, title_case};

use super::{Context, measure};

/// A function that an expression can call.
pub struct Function {
    pub name: &'static str,
    /// The arguments, for the documentation.
    pub signature: &'static str,
    pub min_args: usize,
    /// `None` for no limit.
    pub max_args: Option<usize>,
    pub summary: &'static str,
    pub category: Category,
    /// One short line for the list of functions in the UI.
    pub hint: &'static str,
    pub(super) call: fn(&[Value], &Context) -> Value,
}

impl Function {
    /// The name of argument `i`, from the signature. An argument after the named ones takes the last name.
    pub fn slot(&self, i: usize) -> &'static str {
        let inner =
            self.signature[self.signature.find('(').map_or(0, |p| p + 1)..].trim_end_matches(')');
        let names: Vec<&'static str> = inner
            .split(", ")
            .filter(|n| !n.is_empty() && *n != "...")
            .collect();
        names.get(i).or(names.last()).copied().unwrap_or("")
    }
}

/// A group of functions in the UI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Category {
    Math,
    Text,
    Logic,
    Convert,
    Geometry,
    /// Values of the page of an atlas. Only a text item on a page has them.
    Page,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::Math => "Math",
            Category::Text => "Text",
            Category::Logic => "Logic",
            Category::Convert => "Convert",
            Category::Geometry => "Geometry",
            Category::Page => "Page",
        }
    }
}

/// Two functions are equal when their names are equal.
impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

/// All functions, by name.
pub static FUNCTIONS: &[Function] = &[
    Function {
        name: "if",
        signature: "if(condition, a, b)",
        min_args: 3,
        max_args: Some(3),
        summary: "`a` if the condition is true, else `b`",
        category: Category::Logic,
        hint: "Choose by a condition",
        call: |a, _| {
            if truthy(&a[0]) {
                a[1].clone()
            } else {
                a[2].clone()
            }
        },
    },
    Function {
        name: "coalesce",
        signature: "coalesce(a, b, ...)",
        min_args: 1,
        max_args: None,
        summary: "The first value that is not null",
        category: Category::Logic,
        hint: "First value that is not empty",
        call: |a, _| {
            a.iter()
                .find(|v| **v != Value::Null)
                .cloned()
                .unwrap_or(Value::Null)
        },
    },
    Function {
        name: "lower",
        signature: "lower(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text in lower case",
        category: Category::Text,
        hint: "Text in lower case",
        call: |a, _| map_text(&a[0], |s| s.to_lowercase()),
    },
    Function {
        name: "upper",
        signature: "upper(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text in upper case",
        category: Category::Text,
        hint: "Text in upper case",
        call: |a, _| map_text(&a[0], |s| s.to_uppercase()),
    },
    Function {
        name: "title",
        signature: "title(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text with an upper-case first letter in each word",
        category: Category::Text,
        hint: "Capital first letter in each word",
        call: |a, _| map_text(&a[0], title_case),
    },
    Function {
        name: "trim",
        signature: "trim(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text without spaces at the start and the end",
        category: Category::Text,
        hint: "Remove spaces at the ends",
        call: |a, _| map_text(&a[0], |s| s.trim().to_string()),
    },
    Function {
        name: "length",
        signature: "length(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The number of characters in the text",
        category: Category::Text,
        hint: "Number of characters",
        call: |a, _| match &a[0] {
            Value::Text(s) => Value::Number(s.chars().count() as f64),
            _ => Value::Null,
        },
    },
    Function {
        name: "substr",
        signature: "substr(text, start, length)",
        min_args: 2,
        max_args: Some(3),
        summary: "Part of the text. The first character is 1, and a negative start counts from the end. Without a length, the part goes to the end",
        category: Category::Text,
        hint: "Part of a text",
        call: |a, _| substr(a),
    },
    Function {
        name: "left",
        signature: "left(text, n)",
        min_args: 2,
        max_args: Some(2),
        summary: "The first `n` characters",
        category: Category::Text,
        hint: "First characters",
        call: |a, _| match (&a[0], count(&a[1])) {
            (Value::Text(s), Some(n)) => Value::Text(s.chars().take(n).collect()),
            _ => Value::Null,
        },
    },
    Function {
        name: "right",
        signature: "right(text, n)",
        min_args: 2,
        max_args: Some(2),
        summary: "The last `n` characters",
        category: Category::Text,
        hint: "Last characters",
        call: |a, _| match (&a[0], count(&a[1])) {
            (Value::Text(s), Some(n)) => {
                let len = s.chars().count();
                Value::Text(s.chars().skip(len.saturating_sub(n)).collect())
            }
            _ => Value::Null,
        },
    },
    Function {
        name: "replace",
        signature: "replace(text, find, with)",
        min_args: 3,
        max_args: Some(3),
        summary: "The text with each `find` replaced by `with`",
        category: Category::Text,
        hint: "Replace part of a text",
        call: |a, _| match (&a[0], &a[1], &a[2]) {
            (Value::Text(s), Value::Text(f), Value::Text(w)) if !f.is_empty() => {
                Value::Text(s.replace(f.as_str(), w))
            }
            (Value::Text(s), Value::Text(_), Value::Text(_)) => Value::Text(s.clone()),
            _ => Value::Null,
        },
    },
    Function {
        name: "concat",
        signature: "concat(a, b, ...)",
        min_args: 1,
        max_args: None,
        summary: "The values joined as text. Null values are skipped",
        category: Category::Text,
        hint: "Join values as text",
        call: |a, _| Value::Text(a.iter().filter_map(text_of).collect()),
    },
    Function {
        name: "to_text",
        signature: "to_text(value)",
        min_args: 1,
        max_args: Some(1),
        summary: "The value as text. A whole number has no decimal point",
        category: Category::Convert,
        hint: "Value as text",
        call: |a, _| text_of(&a[0]).map_or(Value::Null, Value::Text),
    },
    Function {
        name: "to_number",
        signature: "to_number(value)",
        min_args: 1,
        max_args: Some(1),
        summary: "The value as a number, or null if the text is not a number. `true` is 1 and `false` is 0",
        category: Category::Convert,
        hint: "Value as a number",
        call: |a, _| match &a[0] {
            Value::Number(n) => Value::Number(*n),
            Value::Bool(b) => Value::Number(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) => s
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map_or(Value::Null, Value::Number),
            Value::Null => Value::Null,
        },
    },
    Function {
        name: "format_number",
        signature: "format_number(x, places)",
        min_args: 1,
        max_args: Some(2),
        summary: "`x` as text with `places` decimal places, 0 if not given, and a comma between each three digits of the whole part",
        category: Category::Convert,
        hint: "Number with thousands separators",
        call: |a, _| {
            let places = match a.get(1) {
                None => 0,
                Some(Value::Number(p)) if p.is_finite() => p.round().clamp(0.0, 15.0) as usize,
                Some(_) => return Value::Null,
            };
            match a[0] {
                Value::Number(x) if x.is_finite() => Value::Text(grouped(x, places)),
                _ => Value::Null,
            }
        },
    },
    Function {
        name: "round",
        signature: "round(x, digits)",
        min_args: 1,
        max_args: Some(2),
        summary: "`x` rounded to `digits` decimal places, 0 if not given",
        category: Category::Math,
        hint: "Round to decimal places",
        call: |a, _| {
            let digits = match a.get(1) {
                None => 0.0,
                Some(Value::Number(d)) => d.round().clamp(-15.0, 15.0),
                Some(_) => return Value::Null,
            };
            let f = 10f64.powf(digits);
            number(&a[0], |x| (x * f).round() / f)
        },
    },
    Function {
        name: "floor",
        signature: "floor(x)",
        min_args: 1,
        max_args: Some(1),
        summary: "The largest whole number not above `x`",
        category: Category::Math,
        hint: "Round down",
        call: |a, _| number(&a[0], f64::floor),
    },
    Function {
        name: "ceil",
        signature: "ceil(x)",
        min_args: 1,
        max_args: Some(1),
        summary: "The smallest whole number not below `x`",
        category: Category::Math,
        hint: "Round up",
        call: |a, _| number(&a[0], f64::ceil),
    },
    Function {
        name: "abs",
        signature: "abs(x)",
        min_args: 1,
        max_args: Some(1),
        summary: "The absolute value of `x`",
        category: Category::Math,
        hint: "Value without a sign",
        call: |a, _| number(&a[0], f64::abs),
    },
    Function {
        name: "min",
        signature: "min(a, b, ...)",
        min_args: 1,
        max_args: None,
        summary: "The smallest number. Values that are not numbers are skipped",
        category: Category::Math,
        hint: "Smallest number",
        call: |a, _| fold_numbers(a, f64::min),
    },
    Function {
        name: "max",
        signature: "max(a, b, ...)",
        min_args: 1,
        max_args: None,
        summary: "The largest number. Values that are not numbers are skipped",
        category: Category::Math,
        hint: "Largest number",
        call: |a, _| fold_numbers(a, f64::max),
    },
    Function {
        name: "clamp",
        signature: "clamp(low, x, high)",
        min_args: 3,
        max_args: Some(3),
        summary: "`x`, but not below `low` and not above `high`",
        category: Category::Math,
        hint: "Keep a number in a range",
        call: |a, _| match (&a[0], &a[1], &a[2]) {
            (Value::Number(lo), Value::Number(x), Value::Number(hi)) => {
                Value::Number(x.max(*lo).min(*hi))
            }
            _ => Value::Null,
        },
    },
    Function {
        name: "area",
        signature: "area(unit)",
        min_args: 0,
        max_args: Some(1),
        summary: "The area of a polygon. The unit is `'km2'`, `'mi2'`, `'ha'`, or `'acre'`, and `'km2'` if not given.",
        category: Category::Geometry,
        hint: "Area of a polygon",
        call: measure::area,
    },
    Function {
        name: "perimeter",
        signature: "perimeter(unit)",
        min_args: 0,
        max_args: Some(1),
        summary: "The length of the edge of a polygon. The unit is `'km'`, `'mi'`, or `'m'`, and `'km'` if not given.",
        category: Category::Geometry,
        hint: "Length of the edge of a polygon",
        call: measure::perimeter,
    },
    Function {
        name: "line_length",
        signature: "line_length(unit)",
        min_args: 0,
        max_args: Some(1),
        summary: "The length of a line, with the units of `perimeter`",
        category: Category::Geometry,
        hint: "Length of a line",
        call: measure::line_length,
    },
    Function {
        name: "scale",
        signature: "scale(format, unit)",
        min_args: 1,
        max_args: Some(2),
        summary: "The scale of the map frame. `format` is `'fraction'` for `1:25,000,000`, or `'relative'` for `1 cm = 250 km`. `unit` is `'cm'` or `'in'`, and `'cm'` if not given",
        category: Category::Page,
        hint: "Scale of the map",
        call: |_, _| Value::Null,
    },
    Function {
        name: "map_name",
        signature: "map_name()",
        min_args: 0,
        max_args: Some(0),
        summary: "The name of the map in the map frame",
        category: Category::Page,
        hint: "Name of the map",
        call: |_, _| Value::Null,
    },
    Function {
        name: "page_name",
        signature: "page_name()",
        min_args: 0,
        max_args: Some(0),
        summary: "The name of the page",
        category: Category::Page,
        hint: "Name of the page",
        call: |_, _| Value::Null,
    },
    Function {
        name: "page_number",
        signature: "page_number()",
        min_args: 0,
        max_args: Some(0),
        summary: "The number of the page in the atlas. The first page is 1",
        category: Category::Page,
        hint: "Number of the page",
        call: |_, _| Value::Null,
    },
    Function {
        name: "page_count",
        signature: "page_count()",
        min_args: 0,
        max_args: Some(0),
        summary: "The number of pages in the atlas",
        category: Category::Page,
        hint: "Number of pages",
        call: |_, _| Value::Null,
    },
    Function {
        name: "date",
        signature: "date(format)",
        min_args: 1,
        max_args: Some(1),
        summary: "The date of today. `format` is `'long'` for `November 8, 2026`, or `'iso'` for `2026-11-08`",
        category: Category::Page,
        hint: "Date of today",
        call: |_, _| Value::Null,
    },
];

/// The function with this name. Case does not matter.
pub fn find(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// The functions that a layer expression can use: all functions but the page functions.
pub fn layer_functions() -> impl Iterator<Item = &'static Function> {
    FUNCTIONS.iter().filter(|f| f.category != Category::Page)
}

/// Only `true` is true.
pub(super) fn truthy(v: &Value) -> bool {
    matches!(v, Value::Bool(true))
}

/// The value as text, or `None` for null.
pub(super) fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(number_text(*n)),
        Value::Text(s) => Some(s.clone()),
    }
}

/// A number as an expression writes it. A whole number has no decimal point.
pub(super) fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// The number rounded half away from zero to `places`, with a comma between each three digits of the whole part.
fn grouped(x: f64, places: usize) -> String {
    let f = 10f64.powi(places as i32);
    let mut r = (x * f).round() / f;
    if !r.is_finite() {
        r = x;
    }
    let digits = format!("{:.places$}", r.abs());
    let (whole, fraction) = digits.split_at(digits.find('.').unwrap_or(digits.len()));
    let mut out = String::new();
    if r < 0.0 {
        out.push('-');
    }
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.push_str(fraction);
    out
}

fn map_text(v: &Value, f: impl Fn(&str) -> String) -> Value {
    match v {
        Value::Text(s) => Value::Text(f(s)),
        _ => Value::Null,
    }
}

fn number(v: &Value, f: impl Fn(f64) -> f64) -> Value {
    match v {
        Value::Number(n) => finite(f(*n)),
        _ => Value::Null,
    }
}

/// A number, or null if it is not finite.
pub(super) fn finite(n: f64) -> Value {
    if n.is_finite() {
        Value::Number(n)
    } else {
        Value::Null
    }
}

fn fold_numbers(a: &[Value], f: fn(f64, f64) -> f64) -> Value {
    a.iter()
        .filter_map(Value::as_f64)
        .reduce(f)
        .map_or(Value::Null, Value::Number)
}

/// A count of characters. A negative count is 0.
fn count(v: &Value) -> Option<usize> {
    match v {
        Value::Number(n) if n.is_finite() => Some(n.max(0.0).min(usize::MAX as f64) as usize),
        _ => None,
    }
}

fn substr(a: &[Value]) -> Value {
    let Value::Text(s) = &a[0] else {
        return Value::Null;
    };
    let Value::Number(start) = a[1] else {
        return Value::Null;
    };
    if !start.is_finite() {
        return Value::Null;
    }
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as f64;
    let from = if start < 0.0 {
        (len + start).max(0.0)
    } else {
        (start - 1.0).max(0.0)
    }
    .min(len) as usize;
    let to = match a.get(2) {
        None => chars.len(),
        Some(v) => match count(v) {
            Some(n) => from.saturating_add(n).min(chars.len()),
            None => return Value::Null,
        },
    };
    Value::Text(chars[from..to].iter().collect())
}
