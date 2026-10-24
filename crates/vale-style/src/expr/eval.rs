//! Evaluates a syntax tree.

use std::cmp::Ordering;

use vale_store::Value;

use super::functions::{finite, text_of};
use super::parser::{BinOp, Node, Var};
use super::Context;

pub(super) fn eval(node: &Node, ctx: &Context) -> Value {
    match node {
        Node::Lit(v) => v.clone(),
        Node::Field(name) => ctx.attributes.get(name).cloned().unwrap_or(Value::Null),
        Node::Var(Var::Scale) => finite(ctx.scale),
        Node::Var(Var::Geometry) => Value::Text(ctx.geometry.name().to_string()),
        Node::Not(inner) => match eval(inner, ctx) {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Null,
        },
        Node::Neg(inner) => match eval(inner, ctx) {
            Value::Number(n) => Value::Number(-n),
            _ => Value::Null,
        },
        Node::Binary(BinOp::And, a, b) => logic(eval(a, ctx), || eval(b, ctx), false),
        Node::Binary(BinOp::Or, a, b) => logic(eval(a, ctx), || eval(b, ctx), true),
        Node::Binary(op, a, b) => binary(*op, eval(a, ctx), eval(b, ctx)),
        Node::IsNull { value, negated } => {
            Value::Bool((eval(value, ctx) == Value::Null) != *negated)
        }
        Node::In {
            value,
            list,
            negated,
        } => {
            let v = eval(value, ctx);
            if v == Value::Null {
                return Value::Null;
            }
            let found = list
                .iter()
                .any(|item| compare(&v, &eval(item, ctx)) == Some(Ordering::Equal));
            Value::Bool(found != *negated)
        }
        Node::Like {
            value,
            pattern,
            ignore_case,
            negated,
        } => match (eval(value, ctx), eval(pattern, ctx)) {
            (Value::Text(s), Value::Text(p)) => {
                let (s, p) = if *ignore_case {
                    (s.to_lowercase(), p.to_lowercase())
                } else {
                    (s, p)
                };
                let s: Vec<char> = s.chars().collect();
                let p: Vec<char> = p.chars().collect();
                Value::Bool(like(&s, &p) != *negated)
            }
            _ => Value::Null,
        },
        Node::Call(f, args) => {
            let args: Vec<Value> = args.iter().map(|a| eval(a, ctx)).collect();
            (f.call)(&args)
        }
    }
}

/// Three-valued `and` and `or`. `short` is the value that decides the result.
fn logic(a: Value, b: impl FnOnce() -> Value, short: bool) -> Value {
    let a = as_bool(&a);
    if a == Some(short) {
        return Value::Bool(short);
    }
    let b = as_bool(&b());
    if b == Some(short) {
        return Value::Bool(short);
    }
    match (a, b) {
        (Some(_), Some(_)) => Value::Bool(!short),
        _ => Value::Null,
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn binary(op: BinOp, a: Value, b: Value) -> Value {
    match op {
        BinOp::Concat => match (text_of(&a), text_of(&b)) {
            (Some(a), Some(b)) => Value::Text(a + &b),
            _ => Value::Null,
        },
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
            let (Value::Number(x), Value::Number(y)) = (a, b) else {
                return Value::Null;
            };
            finite(match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => x / y,
                _ => x % y,
            })
        }
        _ => {
            let Some(ord) = compare(&a, &b) else {
                return Value::Null;
            };
            Value::Bool(match op {
                BinOp::Eq => ord == Ordering::Equal,
                BinOp::Ne => ord != Ordering::Equal,
                BinOp::Lt => ord == Ordering::Less,
                BinOp::Le => ord != Ordering::Greater,
                BinOp::Gt => ord == Ordering::Greater,
                _ => ord != Ordering::Less,
            })
        }
    }
}

/// Values of the same type compare. All other pairs give `None`.
fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// Matches `s` against a pattern with `%` for any run of characters and `_` for one.
fn like(s: &[char], p: &[char]) -> bool {
    // The position after the last `%`, and the text position that it matched up to.
    let (mut si, mut pi) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '_' || (p[pi] != '%' && p[pi] == s[si])) {
            si += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '%' {
            pi += 1;
            star = Some((pi, si));
        } else if let Some((sp, ss)) = star {
            pi = sp;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '%')
}
