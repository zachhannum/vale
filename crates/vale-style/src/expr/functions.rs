//! The functions of the expression language.

use vale_store::Value;

/// A function that an expression can call.
pub struct Function {
    pub name: &'static str,
    /// The arguments, for the documentation.
    pub signature: &'static str,
    pub min_args: usize,
    /// `None` for no limit.
    pub max_args: Option<usize>,
    pub summary: &'static str,
    pub(super) call: fn(&[Value]) -> Value,
}

/// All functions, by name.
pub static FUNCTIONS: &[Function] = &[
    Function {
        name: "if",
        signature: "if(condition, a, b)",
        min_args: 3,
        max_args: Some(3),
        summary: "`a` if the condition is true, else `b`",
        call: |a| {
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
        call: |a| {
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
        call: |a| map_text(&a[0], |s| s.to_lowercase()),
    },
    Function {
        name: "upper",
        signature: "upper(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text in upper case",
        call: |a| map_text(&a[0], |s| s.to_uppercase()),
    },
    Function {
        name: "title",
        signature: "title(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text with an upper-case first letter in each word",
        call: |a| map_text(&a[0], title_case),
    },
    Function {
        name: "trim",
        signature: "trim(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The text without spaces at the start and the end",
        call: |a| map_text(&a[0], |s| s.trim().to_string()),
    },
    Function {
        name: "length",
        signature: "length(text)",
        min_args: 1,
        max_args: Some(1),
        summary: "The number of characters in the text",
        call: |a| match &a[0] {
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
        call: substr,
    },
    Function {
        name: "left",
        signature: "left(text, n)",
        min_args: 2,
        max_args: Some(2),
        summary: "The first `n` characters",
        call: |a| match (&a[0], count(&a[1])) {
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
        call: |a| match (&a[0], count(&a[1])) {
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
        call: |a| match (&a[0], &a[1], &a[2]) {
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
        call: |a| Value::Text(a.iter().filter_map(text_of).collect()),
    },
    Function {
        name: "to_text",
        signature: "to_text(value)",
        min_args: 1,
        max_args: Some(1),
        summary: "The value as text. A whole number has no decimal point",
        call: |a| text_of(&a[0]).map_or(Value::Null, Value::Text),
    },
    Function {
        name: "to_number",
        signature: "to_number(value)",
        min_args: 1,
        max_args: Some(1),
        summary: "The value as a number, or null if the text is not a number. `true` is 1 and `false` is 0",
        call: |a| match &a[0] {
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
        name: "round",
        signature: "round(x, digits)",
        min_args: 1,
        max_args: Some(2),
        summary: "`x` rounded to `digits` decimal places, 0 if not given",
        call: |a| {
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
        call: |a| number(&a[0], f64::floor),
    },
    Function {
        name: "ceil",
        signature: "ceil(x)",
        min_args: 1,
        max_args: Some(1),
        summary: "The smallest whole number not below `x`",
        call: |a| number(&a[0], f64::ceil),
    },
    Function {
        name: "abs",
        signature: "abs(x)",
        min_args: 1,
        max_args: Some(1),
        summary: "The absolute value of `x`",
        call: |a| number(&a[0], f64::abs),
    },
    Function {
        name: "min",
        signature: "min(a, b, ...)",
        min_args: 1,
        max_args: None,
        summary: "The smallest number. Values that are not numbers are skipped",
        call: |a| fold_numbers(a, f64::min),
    },
    Function {
        name: "max",
        signature: "max(a, b, ...)",
        min_args: 1,
        max_args: None,
        summary: "The largest number. Values that are not numbers are skipped",
        call: |a| fold_numbers(a, f64::max),
    },
    Function {
        name: "clamp",
        signature: "clamp(low, x, high)",
        min_args: 3,
        max_args: Some(3),
        summary: "`x`, but not below `low` and not above `high`",
        call: |a| match (&a[0], &a[1], &a[2]) {
            (Value::Number(lo), Value::Number(x), Value::Number(hi)) => {
                Value::Number(x.max(*lo).min(*hi))
            }
            _ => Value::Null,
        },
    },
];

/// The function with this name. Case does not matter.
pub fn find(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
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
        Value::Number(n) => Some(format_number(*n)),
        Value::Text(s) => Some(s.clone()),
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
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

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
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
