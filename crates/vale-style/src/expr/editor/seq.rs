//! Values as tokens and blocks.

use std::ops::Range;

use vale_store::Value;

use super::super::field_ref;
use super::super::functions::Function;
use super::super::parser::{BinOp, Node, Spans};
use super::super::print::{self, NOT, UNARY, literal, operand_prec, prec, quote};
use super::cond::{Cond, cond_of};
use super::{SpanMap, read, tolerant};

/// An operator token.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Or,
    And,
    Not,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Add,
    /// Subtraction, or the negative of the value after it.
    Sub,
    Mul,
    Div,
    Rem,
}

impl Op {
    /// The operator as an expression writes it.
    pub fn symbol(self) -> &'static str {
        match self {
            Op::Or => "or",
            Op::And => "and",
            Op::Not => "not",
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Concat => "||",
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Rem => "%",
        }
    }

    fn of(op: BinOp) -> Op {
        match op {
            BinOp::Or => Op::Or,
            BinOp::And => Op::And,
            BinOp::Eq => Op::Eq,
            BinOp::Ne => Op::Ne,
            BinOp::Lt => Op::Lt,
            BinOp::Le => Op::Le,
            BinOp::Gt => Op::Gt,
            BinOp::Ge => Op::Ge,
            BinOp::Concat => Op::Concat,
            BinOp::Add => Op::Add,
            BinOp::Sub => Op::Sub,
            BinOp::Mul => Op::Mul,
            BinOp::Div => Op::Div,
            BinOp::Rem => Op::Rem,
        }
    }
}

/// One token of a value.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Field(String),
    Num(f64),
    Text(String),
    Bool(bool),
    Op(Op),
    Paren(Seq),
    /// A function block. Each argument is a slot.
    Call {
        f: &'static Function,
        args: Vec<Seq>,
    },
    /// A value chosen by conditions. It writes nested `if` calls.
    Cases {
        cases: Vec<(Cond, Seq)>,
        otherwise: Seq,
    },
    /// Expression text that is not one of the other tokens.
    Code(String),
}

impl Tok {
    pub fn to_text(&self) -> String {
        match self {
            Tok::Field(name) => field_ref(name),
            Tok::Num(n) => literal(&Value::Number(*n)),
            Tok::Text(s) => quote(s),
            Tok::Bool(b) => b.to_string(),
            Tok::Op(op) => op.symbol().to_owned(),
            Tok::Paren(seq) => format!("({})", seq.to_text()),
            Tok::Call { f, args } => {
                let args: Vec<String> = args.iter().map(Seq::to_text).collect();
                format!("{}({})", f.name, args.join(", "))
            }
            Tok::Cases { cases, otherwise } => {
                cases.iter().rev().fold(otherwise.to_text(), |acc, (c, v)| {
                    format!("if({}, {}, {acc})", c.to_text(), v.to_text())
                })
            }
            Tok::Code(s) => s.clone(),
        }
    }
}

/// A run of tokens. An empty sequence is a hole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seq(pub Vec<Tok>);

impl Seq {
    pub fn to_text(&self) -> String {
        let parts: Vec<String> = self.0.iter().map(Tok::to_text).collect();
        parts.join(" ")
    }

    pub fn is_hole(&self) -> bool {
        self.0.is_empty()
    }
}

/// A value, such as a style property, as tokens and blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct Formula {
    pub seq: Seq,
    spans: SpanMap,
}

impl Formula {
    pub fn new(seq: Seq) -> Self {
        Formula {
            seq,
            spans: SpanMap::default(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut spans = SpanMap::default();
        let seq = match read(text) {
            Some((node, sp)) => seq_of(&node, &sp, &mut Vec::new(), &mut spans),
            None => tolerant::formula(text, &mut spans),
        };
        Formula { seq, spans }
    }

    pub fn to_text(&self) -> String {
        self.seq.to_text()
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

/// The tokens of a node. `path` is the path of the sequence.
pub(super) fn seq_of(node: &Node, sp: &Spans, path: &mut Vec<usize>, spans: &mut SpanMap) -> Seq {
    spans.add(path, sp.span.clone());
    let mut out = Reader {
        toks: Vec::new(),
        path,
        spans,
    };
    out.node(node, sp);
    Seq(out.toks)
}

struct Reader<'a> {
    toks: Vec<Tok>,
    /// The path of the sequence.
    path: &'a mut Vec<usize>,
    spans: &'a mut SpanMap,
}

impl Reader<'_> {
    /// Adds a token, and gives the path of the token.
    fn push(&mut self, span: Range<usize>, tok: impl FnOnce(&mut Vec<usize>, &mut SpanMap) -> Tok) {
        self.path.push(self.toks.len());
        self.spans.add(self.path, span);
        let tok = tok(self.path, self.spans);
        self.path.pop();
        self.toks.push(tok);
    }

    fn node(&mut self, node: &Node, sp: &Spans) {
        let span = sp.span.clone();
        match node {
            Node::Hole => {}
            Node::Binary(op, a, b) => {
                let (ka, kb) = (&sp.kids[0], &sp.kids[1]);
                self.operand(a, ka, operand_prec(*op, false));
                self.push(ka.span.end..kb.span.start, |_, _| Tok::Op(Op::of(*op)));
                self.operand(b, kb, operand_prec(*op, true));
            }
            Node::Not(a) => {
                self.push(span.start..sp.kids[0].span.start, |_, _| Tok::Op(Op::Not));
                self.operand(a, &sp.kids[0], NOT);
            }
            Node::Neg(a) => match &**a {
                Node::Lit(Value::Number(n)) => self.push(span, |_, _| Tok::Num(-n)),
                _ => {
                    self.push(span.start..sp.kids[0].span.start, |_, _| Tok::Op(Op::Sub));
                    self.operand(a, &sp.kids[0], UNARY);
                }
            },
            Node::Lit(Value::Number(n)) => self.push(span, |_, _| Tok::Num(*n)),
            Node::Lit(Value::Text(s)) => self.push(span, |_, _| Tok::Text(s.clone())),
            Node::Lit(Value::Bool(b)) => self.push(span, |_, _| Tok::Bool(*b)),
            Node::Field(name) => self.push(span, |_, _| Tok::Field(name.clone())),
            Node::Call(f, args) if f.name == "if" && args.len() == 3 => {
                self.push(span, |path, spans| cases(node, sp, path, spans))
            }
            Node::Call(f, args) => self.push(span, |path, spans| Tok::Call {
                f,
                args: args
                    .iter()
                    .zip(&sp.kids)
                    .enumerate()
                    .map(|(i, (a, s))| {
                        path.push(i);
                        let seq = seq_of(a, s, path, spans);
                        path.pop();
                        seq
                    })
                    .collect(),
            }),
            _ => self.push(span, |_, _| Tok::Code(print::print(node))),
        }
    }

    /// An operand, in parentheses if its level is below `min`.
    fn operand(&mut self, node: &Node, sp: &Spans, min: u8) {
        if prec(node) < min {
            self.push(sp.span.clone(), |path, spans| {
                path.push(0);
                let seq = seq_of(node, sp, path, spans);
                path.pop();
                Tok::Paren(seq)
            });
        } else {
            self.node(node, sp);
        }
    }
}

/// The Cases block of an `if` call and the `if` calls in its else branch. `path` is the path of the block.
fn cases(node: &Node, sp: &Spans, path: &mut Vec<usize>, spans: &mut SpanMap) -> Tok {
    let mut chain = Vec::new();
    let (mut node, mut sp) = (node, sp);
    while let Node::Call(f, args) = node
        && f.name == "if"
        && args.len() == 3
    {
        chain.push((&args[0], &sp.kids[0], &args[1], &sp.kids[1]));
        (node, sp) = (&args[2], &sp.kids[2]);
    }
    let mut cases = Vec::new();
    for (k, (c, cs, v, vs)) in chain.iter().enumerate() {
        path.push(2 * k);
        let cond = cond_of(c, cs, path, spans);
        path.pop();
        path.push(2 * k + 1);
        let value = seq_of(v, vs, path, spans);
        path.pop();
        cases.push((cond, value));
    }
    path.push(2 * chain.len());
    let otherwise = seq_of(node, sp, path, spans);
    path.pop();
    Tok::Cases { cases, otherwise }
}
