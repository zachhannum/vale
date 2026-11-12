//! Label text as a template of text, fields, and groups.

use std::ops::Range;

use vale_store::Value;

use super::super::field_ref;
use super::super::functions::Category;
use super::super::parser::{BinOp, Node, Spans, Var};
use super::super::print::{CONCAT, quote};
use super::cond::operands;
use super::seq::{Seq, seq_of};
use super::subject::{Subject, subject_of};
use super::{SpanMap, read, tolerant, wrap_code};

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

impl FieldFormat {
    /// The format of a subject that the user inserts: a measure or a raster value with thousands separators and no places, a latitude or a longitude with 2 places, and no format for the others.
    pub fn for_subject(subject: &Subject) -> FieldFormat {
        match subject {
            Subject::Measure(..) | Subject::Raster(..) => FieldFormat {
                places: Some(0),
                thousands: true,
                ..Default::default()
            },
            Subject::Var(Var::Lat | Var::Lon) => FieldFormat {
                places: Some(2),
                ..Default::default()
            },
            Subject::Field(_) | Subject::Var(_) => FieldFormat::default(),
        }
    }
}

/// One piece of a label text.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// Fixed text. An empty text is a boundary between two pieces of text that write as separate texts.
    Text(String),
    LineBreak,
    /// A field, a variable, or a measure, with a format.
    Field {
        subject: Subject,
        format: FieldFormat,
    },
    /// A call of a page function whose arguments are texts, for example `scale('relative', 'cm')`. `name` is the name of the function.
    Page {
        name: &'static str,
        options: Vec<String>,
    },
    /// Pieces that show only when each of them has a value. It writes a chain of `||`.
    Group(Vec<Piece>),
    /// An expression that is not one of the other pieces. An empty expression is a hole.
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
                    Node::Hole => {}
                    Node::Call(f, args) if f.name == "concat" && args.len() > 1 => {
                        for (a, s) in args.iter().zip(&sp.kids) {
                            pieces_of(a, s, &mut pieces, &mut path, &mut spans);
                        }
                    }
                    _ => pieces_of(&node, &sp, &mut pieces, &mut path, &mut spans),
                }
            }
            None => pieces = tolerant::template(text, &mut spans),
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
pub(super) fn pieces_of(
    node: &Node,
    sp: &Spans,
    list: &mut Vec<Piece>,
    path: &mut Vec<usize>,
    spans: &mut SpanMap,
) {
    path.push(list.len());
    match node {
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
        _ => {
            if let Some((name, options)) = page_of(node) {
                spans.add(path, sp.span.clone());
                list.push(Piece::Page { name, options });
            } else if let Some((subject, format)) = field_of(node) {
                spans.add(path, sp.span.clone());
                list.push(Piece::Field { subject, format });
            } else {
                list.push(Piece::Expr(seq_of(node, sp, path, spans)));
            }
        }
    }
    path.pop();
}

/// The name and the options of a call of a page function whose arguments are all texts.
fn page_of(node: &Node) -> Option<(&'static str, Vec<String>)> {
    let Node::Call(f, args) = node else {
        return None;
    };
    if f.category != Category::Page {
        return None;
    }
    let options = args.iter().map(|a| match a {
        Node::Lit(Value::Text(s)) => Some(s.clone()),
        _ => None,
    });
    Some((f.name, options.collect::<Option<_>>()?))
}

fn page_text(name: &str, options: &[String]) -> String {
    let args: Vec<String> = options.iter().map(|o| quote(o)).collect();
    format!("{name}({})", args.join(", "))
}

/// The subject and its format, if the node is a subject in the functions of a format, in their order.
fn field_of(node: &Node) -> Option<(Subject, FieldFormat)> {
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
    Some((subject_of(node)?, format))
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

fn field_text(subject: &Subject, format: &FieldFormat) -> String {
    let mut s = subject.to_text();
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
                    Piece::Field { subject, format } => field_text(subject, format),
                    Piece::Page { name, options } => page_text(name, options),
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
