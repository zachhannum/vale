//! Label text as a template of text, fields, and groups.

use std::ops::Range;

use vale_store::Value;

use super::super::field_ref;
use super::super::parser::{BinOp, Node, Spans};
use super::super::print::{CONCAT, quote};
use super::cond::operands;
use super::seq::{Seq, seq_of};
use super::{SpanMap, read, wrap_code};

/// The most decimal places of a field format.
const MAX_PLACES: i32 = 15;

/// The case of a text field.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Case {
    #[default]
    AsIs,
    Upper,
    Lower,
    Title,
}

/// What a field shows when it has no value.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Empty {
    /// Nothing. In a group, the group hides.
    #[default]
    None,
    Text(String),
    Field(String),
}

/// The format of a field piece. It writes `coalesce(case(trim(number(field))), empty)`, with each function only when the format needs it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldFormat {
    /// Rounds a number to this many decimal places, from -15 to 15. A negative count rounds to tens, hundreds, and so on.
    pub places: Option<i32>,
    /// Writes a number with thousands separators, through `format_number`. The places are then 0 to 15, and 0 if not given.
    pub thousands: bool,
    pub case: Case,
    pub trim: bool,
    pub empty: Empty,
}

/// One piece of a label text.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// Fixed text. An empty text is a boundary between two pieces of text that write as separate texts.
    Text(String),
    LineBreak,
    Field {
        name: String,
        format: FieldFormat,
    },
    /// Pieces that show only when each of them has a value. It writes a chain of `||`.
    Group(Vec<Piece>),
    /// An expression that is not one of the other pieces.
    Expr(Seq),
    /// Expression text that does not parse.
    Code(String),
}

/// A label text. Each argument of a `concat` at the top gives its own pieces.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub pieces: Vec<Piece>,
    spans: SpanMap,
}

impl Template {
    pub fn new(pieces: Vec<Piece>) -> Self {
        Template {
            pieces,
            spans: SpanMap::default(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut spans = SpanMap::default();
        let mut pieces = Vec::new();
        match read(text) {
            Some((node, sp)) => {
                spans.add(&[], sp.span.clone());
                let mut path = Vec::new();
                match &node {
                    Node::Call(f, args) if f.name == "concat" && args.len() > 1 => {
                        for (a, s) in args.iter().zip(&sp.kids) {
                            pieces_of(a, s, &mut pieces, &mut path, &mut spans);
                        }
                    }
                    _ => pieces_of(&node, &sp, &mut pieces, &mut path, &mut spans),
                }
            }
            None => {
                spans.add(&[], 0..text.chars().count());
                spans.add(&[0], 0..text.chars().count());
                pieces.push(Piece::Code(text.to_owned()));
            }
        }
        Template { pieces, spans }
    }

    /// One piece writes alone, and more pieces write as a `concat`.
    pub fn to_text(&self) -> String {
        let args = operands_of(&self.pieces, false);
        match args.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            _ => format!("concat({})", args.join(", ")),
        }
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

/// Adds the pieces of a node to `list`. `path` is the path of the list.
fn pieces_of(
    node: &Node,
    sp: &Spans,
    list: &mut Vec<Piece>,
    path: &mut Vec<usize>,
    spans: &mut SpanMap,
) {
    path.push(list.len());
    match node {
        Node::Hole => {}
        Node::Lit(Value::Text(s)) => {
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    spans.add(path, sp.span.clone());
                    list.push(Piece::LineBreak);
                    *path.last_mut().unwrap() += 1;
                }
                spans.add(path, sp.span.clone());
                list.push(Piece::Text(line.to_owned()));
                *path.last_mut().unwrap() += 1;
            }
        }
        Node::Binary(BinOp::Concat, ..) => {
            spans.add(path, sp.span.clone());
            let mut parts = Vec::new();
            operands(node, sp, BinOp::Concat, &mut parts);
            let mut items = Vec::new();
            for (n, s) in parts {
                pieces_of(n, s, &mut items, path, spans);
            }
            list.push(Piece::Group(items));
        }
        _ => match field_of(node) {
            Some((name, format)) => {
                spans.add(path, sp.span.clone());
                list.push(Piece::Field { name, format });
            }
            None => list.push(Piece::Expr(seq_of(node, sp, path, spans))),
        },
    }
    path.pop();
}

/// The field and its format, if the node is a field in the functions of a format, in their order.
fn field_of(node: &Node) -> Option<(String, FieldFormat)> {
    let mut format = FieldFormat::default();
    let mut node = node;
    if let Some(("coalesce", [x, other])) = call(node) {
        format.empty = match other {
            Node::Lit(Value::Text(s)) => Empty::Text(s.clone()),
            Node::Field(g) => Empty::Field(g.clone()),
            _ => return None,
        };
        node = x;
    }
    if let Some((name @ ("upper" | "lower" | "title"), [x])) = call(node) {
        format.case = match name {
            "upper" => Case::Upper,
            "lower" => Case::Lower,
            _ => Case::Title,
        };
        node = x;
    }
    if let Some(("trim", [x])) = call(node) {
        format.trim = true;
        node = x;
    }
    match call(node) {
        Some(("format_number", [x, p])) => {
            format.places = Some(whole(p).filter(|p| (0..=MAX_PLACES).contains(p))?);
            format.thousands = true;
            node = x;
        }
        Some(("round", [x])) => {
            format.places = Some(0);
            node = x;
        }
        Some(("round", [x, p])) => {
            format.places =
                Some(whole(p).filter(|p| *p != 0 && (-MAX_PLACES..=MAX_PLACES).contains(p))?);
            node = x;
        }
        _ => {}
    }
    match node {
        Node::Field(name) => Some((name.clone(), format)),
        _ => None,
    }
}

fn call(node: &Node) -> Option<(&'static str, &[Node])> {
    match node {
        Node::Call(f, args) => Some((f.name, args.as_slice())),
        _ => None,
    }
}

/// The whole number of a literal.
fn whole(node: &Node) -> Option<i32> {
    let n = match node {
        Node::Lit(Value::Number(n)) => *n,
        Node::Neg(a) => match &**a {
            Node::Lit(Value::Number(n)) => -n,
            _ => return None,
        },
        _ => return None,
    };
    (n.fract() == 0.0 && n.abs() <= MAX_PLACES as f64).then_some(n as i32)
}

fn field_text(name: &str, format: &FieldFormat) -> String {
    let mut s = field_ref(name);
    if format.thousands {
        let places = format.places.unwrap_or(0).clamp(0, MAX_PLACES);
        s = format!("format_number({s}, {places})");
    } else {
        match format.places.map(|p| p.clamp(-MAX_PLACES, MAX_PLACES)) {
            None => {}
            Some(0) => s = format!("round({s})"),
            Some(p) => s = format!("round({s}, {p})"),
        }
    }
    if format.trim {
        s = format!("trim({s})");
    }
    match format.case {
        Case::AsIs => {}
        Case::Upper => s = format!("upper({s})"),
        Case::Lower => s = format!("lower({s})"),
        Case::Title => s = format!("title({s})"),
    }
    match &format.empty {
        Empty::None => s,
        Empty::Text(t) => format!("coalesce({s}, {})", quote(t)),
        Empty::Field(g) => format!("coalesce({s}, {})", field_ref(g)),
    }
}

/// The text of each operand of a list of pieces: an argument of `concat`, or an operand of `||` in a group. A run of text pieces and line breaks in which no two text pieces touch writes as one text.
fn operands_of(pieces: &[Piece], in_group: bool) -> Vec<String> {
    let mut out = Vec::new();
    // The open text, and true if its last piece is a text piece.
    let mut open: Option<(String, bool)> = None;
    let close = |open: &mut Option<(String, bool)>, out: &mut Vec<String>| {
        if let Some((text, _)) = open.take() {
            out.push(quote(&text));
        }
    };
    for p in pieces {
        match p {
            Piece::Text(s) => {
                if matches!(open, Some((_, true))) {
                    close(&mut open, &mut out);
                }
                let (text, last_text) = open.get_or_insert_with(Default::default);
                text.push_str(s);
                *last_text = true;
            }
            Piece::LineBreak => {
                let (text, last_text) = open.get_or_insert_with(Default::default);
                text.push('\n');
                *last_text = false;
            }
            _ => {
                close(&mut open, &mut out);
                let min = if out.is_empty() { CONCAT } else { CONCAT + 1 };
                let text = match p {
                    Piece::Field { name, format } => field_text(name, format),
                    Piece::Group(items) => {
                        let text = operands_of(items, true).join(" || ");
                        if in_group { format!("({text})") } else { text }
                    }
                    Piece::Expr(seq) if in_group => wrap_code(&seq.to_text(), min),
                    Piece::Expr(seq) => seq.to_text(),
                    Piece::Code(s) if in_group => wrap_code(s, min),
                    Piece::Code(s) => s.clone(),
                    Piece::Text(_) | Piece::LineBreak => unreachable!(),
                };
                out.push(text);
            }
        }
    }
    close(&mut open, &mut out);
    out
}
