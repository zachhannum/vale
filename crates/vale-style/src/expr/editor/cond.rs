//! Filter conditions.

use std::ops::Range;

use vale_store::{FieldType, Value};

use super::super::field_ref;
use super::super::functions::number_text;
use super::super::parser::{BinOp, Node, Spans};
use super::super::print::{self, AND, OR, literal, quote};
use super::{SpanMap, read, tolerant, wrap_code};

/// The comparison of a condition.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Is,
    IsNot,
    Gt,
    Ge,
    Lt,
    Le,
    Between,
    Empty,
    NotEmpty,
    /// Text that holds the value, with any case.
    Contains,
    /// Text that starts with the value, with any case.
    StartsWith,
    OneOf,
}

impl CmpOp {
    pub fn label(self) -> &'static str {
        match self {
            CmpOp::Is => "is",
            CmpOp::IsNot => "is not",
            CmpOp::Gt => "is greater than",
            CmpOp::Ge => "is at least",
            CmpOp::Lt => "is less than",
            CmpOp::Le => "is at most",
            CmpOp::Between => "is between",
            CmpOp::Empty => "is empty",
            CmpOp::NotEmpty => "is not empty",
            CmpOp::Contains => "contains",
            CmpOp::StartsWith => "starts with",
            CmpOp::OneOf => "is one of",
        }
    }

    /// The number of values that the comparison takes, or `None` for one or more.
    pub fn value_count(self) -> Option<usize> {
        match self {
            CmpOp::Empty | CmpOp::NotEmpty => Some(0),
            CmpOp::Between => Some(2),
            CmpOp::OneOf => None,
            _ => Some(1),
        }
    }
}

/// The comparisons for a field of this type, in menu order.
pub fn ops_for(field_type: FieldType) -> &'static [CmpOp] {
    use CmpOp::*;
    match field_type {
        FieldType::Integer | FieldType::Real => {
            &[Is, IsNot, Gt, Ge, Lt, Le, Between, OneOf, Empty, NotEmpty]
        }
        FieldType::Text => &[Is, IsNot, Contains, StartsWith, OneOf, Empty, NotEmpty],
        FieldType::Bool => &[Is, Empty, NotEmpty],
    }
}

/// A condition of a filter or of a case.
#[derive(Clone, Debug, PartialEq)]
pub enum Cond {
    /// A field, a comparison, and its values. A yes-or-no field compares with `Value::Bool`. A rule that lacks values writes holes.
    Rule {
        field: String,
        op: CmpOp,
        values: Vec<Value>,
    },
    /// True if all the items are true, or if one item is true. An empty group is a hole.
    Group { all: bool, items: Vec<Cond> },
    /// Expression text that is not a rule.
    Code(String),
}

impl Cond {
    /// An empty condition, which writes as a hole.
    pub fn hole() -> Cond {
        Cond::Group {
            all: true,
            items: Vec::new(),
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Cond::Group { all, items } => group_text(*all, items),
            Cond::Rule { field, op, values } => rule_text(field, *op, values),
            Cond::Code(s) => s.clone(),
        }
    }
}

/// The conditions of a filter. No conditions match each feature.
#[derive(Clone, Debug, PartialEq)]
pub struct Conditions {
    /// True to match a feature when all conditions are true, false when one is true.
    pub all: bool,
    pub items: Vec<Cond>,
    spans: SpanMap,
}

impl Conditions {
    pub fn new(all: bool, items: Vec<Cond>) -> Self {
        Conditions {
            all,
            items,
            spans: SpanMap::default(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut spans = SpanMap::default();
        let cond = match read(text) {
            Some((node, sp)) => cond_of(&node, &sp, &mut Vec::new(), &mut spans),
            None => tolerant::condition(text, &mut spans),
        };
        let (all, items) = match cond {
            Cond::Group { all, items } => (all, items),
            c => {
                if let Some(s) = spans.get(&[]) {
                    spans.add(&[0], s);
                }
                (true, vec![c])
            }
        };
        Conditions { all, items, spans }
    }

    pub fn to_text(&self) -> String {
        group_text(self.all, &self.items)
    }

    /// The span of the node at `path` in the text that the model was read from.
    pub fn span(&self, path: &[usize]) -> Option<Range<usize>> {
        self.spans.get(path)
    }

    /// The path of the smallest node at the char position `pos`, for example the position of a parse error.
    pub fn path_at(&self, pos: usize) -> Option<Vec<usize>> {
        self.spans.at(pos)
    }
}

/// The condition of a node. And and or chains that group from the left become one group. `path` is the path of the node, and `spans` gets the span of each new model node.
pub(super) fn cond_of(node: &Node, sp: &Spans, path: &mut Vec<usize>, spans: &mut SpanMap) -> Cond {
    spans.add(path, sp.span.clone());
    let op = match node {
        Node::Binary(op @ (BinOp::And | BinOp::Or), ..) => *op,
        Node::Hole => return Cond::hole(),
        _ => return rule_of(node).unwrap_or_else(|| Cond::Code(print::print(node))),
    };
    let mut parts = Vec::new();
    operands(node, sp, op, &mut parts);
    let items = parts
        .into_iter()
        .enumerate()
        .map(|(i, (n, s))| {
            path.push(i);
            let c = cond_of(n, s, path, spans);
            path.pop();
            c
        })
        .collect();
    Cond::Group {
        all: op == BinOp::And,
        items,
    }
}

/// The operands of a chain of `op` that groups from the left.
pub(super) fn operands<'a>(
    node: &'a Node,
    sp: &'a Spans,
    op: BinOp,
    out: &mut Vec<(&'a Node, &'a Spans)>,
) {
    match node {
        Node::Binary(o, a, b) if *o == op => {
            operands(a, &sp.kids[0], op, out);
            out.push((b, &sp.kids[1]));
        }
        _ => out.push((node, sp)),
    }
}

fn rule_of(node: &Node) -> Option<Cond> {
    let rule = |field: &str, op, values| {
        Some(Cond::Rule {
            field: field.to_owned(),
            op,
            values,
        })
    };
    match node {
        Node::Field(f) => rule(f, CmpOp::Is, vec![Value::Bool(true)]),
        Node::Not(a) => match &**a {
            Node::Field(f) => rule(f, CmpOp::Is, vec![Value::Bool(false)]),
            _ => None,
        },
        Node::Binary(op, a, b) => {
            let Node::Field(f) = &**a else { return None };
            let op = match op {
                BinOp::Eq => CmpOp::Is,
                BinOp::Ne => CmpOp::IsNot,
                BinOp::Gt => CmpOp::Gt,
                BinOp::Ge => CmpOp::Ge,
                BinOp::Lt => CmpOp::Lt,
                BinOp::Le => CmpOp::Le,
                _ => return None,
            };
            rule(f, op, values_of(&[b])?)
        }
        Node::IsNull { value, negated } => {
            let Node::Field(f) = &**value else {
                return None;
            };
            let op = if *negated {
                CmpOp::NotEmpty
            } else {
                CmpOp::Empty
            };
            rule(f, op, Vec::new())
        }
        Node::Between {
            value,
            low,
            high,
            negated: false,
        } => {
            let Node::Field(f) = &**value else {
                return None;
            };
            rule(f, CmpOp::Between, values_of(&[low, high])?)
        }
        Node::In {
            value,
            list,
            negated: false,
        } => {
            let Node::Field(f) = &**value else {
                return None;
            };
            rule(
                f,
                CmpOp::OneOf,
                values_of(&list.iter().collect::<Vec<_>>())?,
            )
        }
        Node::Like {
            value,
            pattern,
            ignore_case: true,
            negated: false,
        } => {
            let (Node::Field(f), Node::Lit(Value::Text(p))) = (&**value, &**pattern) else {
                return None;
            };
            let plain = |s: &str| !s.contains(['%', '_']);
            let inner = p.strip_prefix('%').and_then(|s| s.strip_suffix('%'));
            match (inner, p.strip_suffix('%')) {
                (Some(v), _) if plain(v) => rule(f, CmpOp::Contains, vec![Value::Text(v.into())]),
                (_, Some(v)) if plain(v) => rule(f, CmpOp::StartsWith, vec![Value::Text(v.into())]),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The values of literal nodes. Holes can follow the values, and a hole drops out.
fn values_of(nodes: &[&Node]) -> Option<Vec<Value>> {
    let mut out = Vec::new();
    let mut hole = false;
    for n in nodes {
        match n {
            Node::Hole => hole = true,
            _ if hole => return None,
            Node::Lit(v @ (Value::Number(_) | Value::Text(_) | Value::Bool(_))) => {
                out.push(v.clone())
            }
            Node::Neg(a) => match &**a {
                Node::Lit(Value::Number(x)) => out.push(Value::Number(-x)),
                _ => return None,
            },
            _ => return None,
        }
    }
    Some(out)
}

fn group_text(all: bool, items: &[Cond]) -> String {
    match items {
        [] => String::new(),
        [c] => c.to_text(),
        _ => {
            let parts: Vec<String> = items.iter().map(|c| item_text(c, all)).collect();
            parts.join(if all { " and " } else { " or " })
        }
    }
}

/// An item of a group of two or more, with parentheses where it needs them.
fn item_text(c: &Cond, parent_all: bool) -> String {
    match c {
        Cond::Group { items, .. } if items.len() < 2 => items
            .first()
            .map_or(String::new(), |c| item_text(c, parent_all)),
        Cond::Group { all, items } => {
            let text = group_text(*all, items);
            if *all && !parent_all {
                text
            } else {
                format!("({text})")
            }
        }
        Cond::Rule { .. } => c.to_text(),
        Cond::Code(s) => wrap_code(s, if parent_all { AND + 1 } else { OR + 1 }),
    }
}

fn rule_text(field: &str, op: CmpOp, values: &[Value]) -> String {
    let f = field_ref(field);
    let v = |i: usize| values.get(i).map_or(String::new(), literal);
    let pattern = |before: &str| {
        let text = match values.first() {
            Some(Value::Text(s)) => s.clone(),
            Some(Value::Number(n)) => number_text(*n),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        };
        quote(&format!("{before}{text}%"))
    };
    match op {
        CmpOp::Is => match values {
            [Value::Bool(true)] => f,
            [Value::Bool(false)] => format!("not {f}"),
            _ => format!("{f} = {}", v(0)),
        },
        CmpOp::IsNot => format!("{f} != {}", v(0)),
        CmpOp::Gt => format!("{f} > {}", v(0)),
        CmpOp::Ge => format!("{f} >= {}", v(0)),
        CmpOp::Lt => format!("{f} < {}", v(0)),
        CmpOp::Le => format!("{f} <= {}", v(0)),
        CmpOp::Between => format!("{f} between {} and {}", v(0), v(1)),
        CmpOp::Empty => format!("{f} is null"),
        CmpOp::NotEmpty => format!("{f} is not null"),
        CmpOp::Contains => format!("{f} ilike {}", pattern("%")),
        CmpOp::StartsWith => format!("{f} ilike {}", pattern("")),
        CmpOp::OneOf => {
            let items: Vec<String> = values.iter().map(literal).collect();
            format!("{f} in ({})", items.join(", "))
        }
    }
}
