//! Writes a syntax tree as canonical expression text.

use vale_store::Value;

use super::field_ref;
use super::functions::number_text;
use super::parser::{BinOp, Node, Var};

/// The binding strength of each level of the grammar, from loose to tight.
pub(super) const OR: u8 = 1;
pub(super) const AND: u8 = 2;
pub(super) const NOT: u8 = 3;
pub(super) const COMPARE: u8 = 4;
pub(super) const CONCAT: u8 = 5;
pub(super) const UNARY: u8 = 8;
const PRIMARY: u8 = 9;

pub(super) fn op_prec(op: BinOp) -> u8 {
    match op {
        BinOp::Or => OR,
        BinOp::And => AND,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => COMPARE,
        BinOp::Concat => CONCAT,
        BinOp::Add | BinOp::Sub => 6,
        BinOp::Mul | BinOp::Div | BinOp::Rem => 7,
    }
}

pub(super) fn op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "or",
        BinOp::And => "and",
        BinOp::Eq => "=",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Concat => "||",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
    }
}

/// The level of the grammar that the node belongs to.
pub(super) fn prec(node: &Node) -> u8 {
    match node {
        Node::Binary(op, ..) => op_prec(*op),
        Node::Not(_) => NOT,
        Node::IsNull { .. } | Node::In { .. } | Node::Like { .. } | Node::Between { .. } => COMPARE,
        Node::Neg(_) => UNARY,
        Node::Lit(Value::Number(n)) if n.is_sign_negative() => UNARY,
        _ => PRIMARY,
    }
}

/// The least level that each operand of a binary operator needs to go without parentheses. A comparison takes one operator, and a chain of the other operators groups from the left.
pub(super) fn operand_prec(op: BinOp, right: bool) -> u8 {
    let p = op_prec(op);
    if p == COMPARE {
        CONCAT
    } else if right {
        p + 1
    } else {
        p
    }
}

pub(super) fn print(node: &Node) -> String {
    let mut out = String::new();
    write(node, &mut out);
    out
}

fn write_at(node: &Node, min: u8, out: &mut String) {
    if prec(node) < min {
        out.push('(');
        write(node, out);
        out.push(')');
    } else {
        write(node, out);
    }
}

fn write(node: &Node, out: &mut String) {
    match node {
        Node::Lit(v) => out.push_str(&literal(v)),
        Node::Hole => {}
        Node::Field(name) => out.push_str(&field_ref(name)),
        Node::Var(Var::Scale) => out.push_str("@scale"),
        Node::Var(Var::Geometry) => out.push_str("@geometry"),
        Node::Not(a) => {
            out.push_str("not ");
            write_at(a, NOT, out);
        }
        Node::Neg(a) => {
            out.push('-');
            write_at(a, UNARY, out);
        }
        Node::Binary(op, a, b) => {
            write_at(a, operand_prec(*op, false), out);
            out.push(' ');
            out.push_str(op_text(*op));
            out.push(' ');
            write_at(b, operand_prec(*op, true), out);
        }
        Node::IsNull { value, negated } => {
            write_at(value, CONCAT, out);
            out.push_str(if *negated { " is not null" } else { " is null" });
        }
        Node::In {
            value,
            list,
            negated,
        } => {
            write_at(value, CONCAT, out);
            out.push_str(if *negated { " not in (" } else { " in (" });
            write_list(list, out);
            out.push(')');
        }
        Node::Like {
            value,
            pattern,
            ignore_case,
            negated,
        } => {
            write_at(value, CONCAT, out);
            out.push_str(if *negated { " not " } else { " " });
            out.push_str(if *ignore_case { "ilike " } else { "like " });
            write_at(pattern, CONCAT, out);
        }
        Node::Between {
            value,
            low,
            high,
            negated,
        } => {
            write_at(value, CONCAT, out);
            out.push_str(if *negated {
                " not between "
            } else {
                " between "
            });
            write_at(low, CONCAT, out);
            out.push_str(" and ");
            write_at(high, CONCAT, out);
        }
        Node::Call(f, args) => {
            out.push_str(f.name);
            out.push('(');
            write_list(args, out);
            out.push(')');
        }
    }
}

fn write_list(items: &[Node], out: &mut String) {
    for (i, n) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write(n, out);
    }
}

/// A value as an expression writes it. A number that is not finite writes as an exponent that overflows.
pub(super) fn literal(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) if n.is_nan() => "null".into(),
        Value::Number(n) if n.is_infinite() => if *n > 0.0 { "1e999" } else { "-1e999" }.into(),
        Value::Number(n) if *n == 0.0 && n.is_sign_negative() => "-0".into(),
        Value::Number(n) => number_text(*n),
        Value::Text(s) => quote(s),
    }
}

pub(super) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
