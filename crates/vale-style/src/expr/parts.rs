//! Label text as a list of parts, for the text builder.

use vale_store::Value;

use super::lexer::{self, Tok};
use super::parser::{BinOp, Node};
use super::{Expr, field_ref};

/// The most decimal places that a field part reads.
const MAX_DIGITS: f64 = 15.0;

/// One part of a label text.
#[derive(Clone, Debug, PartialEq)]
pub enum TextPart {
    /// The value of a field. The prefix and the suffix show only when the field has a value. `digits` rounds a number to that many decimal places.
    Field {
        name: String,
        prefix: String,
        suffix: String,
        digits: Option<u32>,
    },
    /// A fixed text.
    Text(String),
    /// Expression text that is not one of the other parts.
    Raw(String),
}

/// The parts of a label text. Each argument of a `concat` at the top is one part. Text that does not parse is one raw part.
pub fn split(text: &str) -> Vec<TextPart> {
    if text.is_empty() {
        return Vec::new();
    }
    let Ok(expr) = Expr::parse(text) else {
        return vec![TextPart::Raw(text.to_owned())];
    };
    if matches!(&expr.root, Node::Call(f, _) if f.name == "concat")
        && let Some(args) = concat_args(text)
    {
        return args.iter().map(|a| part(a)).collect();
    }
    vec![classify(&expr.root).unwrap_or_else(|| TextPart::Raw(text.to_owned()))]
}

/// The label text of the parts. More than one part gives a `concat`.
pub fn join(parts: &[TextPart]) -> String {
    match parts {
        [] => String::new(),
        [p] => write(p),
        _ => {
            let args: Vec<String> = parts.iter().map(write).collect();
            format!("concat({})", args.join(", "))
        }
    }
}

fn write(part: &TextPart) -> String {
    match part {
        TextPart::Field {
            name,
            prefix,
            suffix,
            digits,
        } => {
            let mut out = String::new();
            if !prefix.is_empty() {
                out.push_str(&quote(prefix));
                out.push_str(" || ");
            }
            match digits {
                None => out.push_str(&field_ref(name)),
                Some(0) => out.push_str(&format!("round({})", field_ref(name))),
                Some(n) => out.push_str(&format!("round({}, {n})", field_ref(name))),
            }
            if !suffix.is_empty() {
                out.push_str(" || ");
                out.push_str(&quote(suffix));
            }
            out
        }
        TextPart::Text(s) => quote(s),
        TextPart::Raw(s) => s.clone(),
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The source text of each argument of `concat(...)` when the call is the whole text.
fn concat_args(text: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let tokens = lexer::lex(&chars).ok()?;
    match tokens.as_slice() {
        [first, open, ..]
            if matches!(&first.tok, Tok::Name(n) if n.eq_ignore_ascii_case("concat"))
                && open.tok == Tok::LParen => {}
        _ => return None,
    }
    let mut args = Vec::new();
    let mut depth = 0;
    let mut arg_start: Option<usize> = None;
    let mut arg_end = 0;
    for (i, t) in tokens.iter().enumerate().skip(2) {
        match t.tok {
            Tok::Comma | Tok::RParen if depth == 0 => {
                args.push(chars[arg_start?..arg_end].iter().collect());
                arg_start = None;
                if t.tok == Tok::RParen {
                    return (tokens[i + 1].tok == Tok::End).then_some(args);
                }
                continue;
            }
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::End => return None,
            _ => {}
        }
        arg_start.get_or_insert(t.start);
        arg_end = t.end;
    }
    None
}

fn part(source: &str) -> TextPart {
    Expr::parse(source)
        .ok()
        .and_then(|e| classify(&e.root))
        .unwrap_or_else(|| TextPart::Raw(source.to_owned()))
}

fn classify(node: &Node) -> Option<TextPart> {
    if let Node::Lit(Value::Text(s)) = node {
        return Some(TextPart::Text(s.clone()));
    }
    let mut items = Vec::new();
    flatten(node, &mut items);
    let text = |n: &Node| match n {
        Node::Lit(Value::Text(s)) => Some(s.clone()),
        _ => None,
    };
    let (prefix, core, suffix) = match items.as_slice() {
        [core] => (String::new(), *core, String::new()),
        [a, b] => match (text(a), text(b)) {
            (Some(p), None) => (p, *b, String::new()),
            (None, Some(s)) => (String::new(), *a, s),
            _ => return None,
        },
        [a, core, b] => (text(a)?, *core, text(b)?),
        _ => return None,
    };
    let (name, digits) = field_core(core)?;
    Some(TextPart::Field {
        name,
        prefix,
        suffix,
        digits,
    })
}

/// The operands of a chain of `||`.
fn flatten<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    match node {
        Node::Binary(BinOp::Concat, a, b) => {
            flatten(a, out);
            flatten(b, out);
        }
        _ => out.push(node),
    }
}

/// A field, or a field in `round`.
fn field_core(node: &Node) -> Option<(String, Option<u32>)> {
    match node {
        Node::Field(name) => Some((name.clone(), None)),
        Node::Call(f, args) if f.name == "round" => match args.as_slice() {
            [Node::Field(name)] => Some((name.clone(), Some(0))),
            [Node::Field(name), Node::Lit(Value::Number(d))]
                if d.fract() == 0.0 && (0.0..=MAX_DIGITS).contains(d) =>
            {
                Some((name.clone(), Some(*d as u32)))
            }
            _ => None,
        },
        _ => None,
    }
}
