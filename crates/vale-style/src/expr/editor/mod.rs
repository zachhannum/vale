//! Models of expression text for the chip editor: a label text template, a list of filter conditions, and a value as tokens and blocks.
//!
//! Each model reads from text and writes text. For text that parses, the written text parses to the same syntax tree. The one exception is a comparison of a field with `true` or `false`: `f = true` writes as `f`, and `f = false` writes as `not f`.
//!
//! A path addresses one node of a model. It is a list of child indices from the root, and the empty path is the root. In a list (the pieces of a template or a group, the conditions of a group, the tokens of a sequence), the index picks an item. In a function call, the index picks an argument. In a Cases block with `n` cases, index `2k` picks the condition of case `k`, index `2k + 1` its value, and index `2n` the otherwise value. In parentheses, index 0 picks the inner sequence. An expression piece of a template has the tokens of its sequence as children.
//!
//! Text that does not parse splits at its separators: the arguments of a `concat` or the operands of `||` in a template, `and` and `or` in conditions, and the arguments of a call in a value. Each part that parses becomes model nodes, and each other part becomes code.
//!
//! A model that comes from text keeps the span of each node in the text, in chars. A model that you change keeps the old spans until you read it again.

mod cond;
mod seq;
mod subject;
mod template;
mod tolerant;

use std::ops::Range;

pub use cond::{CmpOp, Cond, Conditions, ops_for};
pub use seq::{Formula, Op, Seq, Tok};
pub use subject::{Subject, measures_for, rasters_for};
pub use template::{Case, Empty, FieldFormat, Piece, Template};

use super::parser::Node;
use super::print::prec;
use super::{Expr, parse_spanned};

/// The span of each node of a model, by path.
#[derive(Clone, Debug, Default)]
struct SpanMap(Vec<(Vec<usize>, Range<usize>)>);

/// Spans do not take part in the equality of models.
impl PartialEq for SpanMap {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl SpanMap {
    fn add(&mut self, path: &[usize], span: Range<usize>) {
        self.0.push((path.to_vec(), span));
    }

    fn get(&self, path: &[usize]) -> Option<Range<usize>> {
        self.0
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, s)| s.clone())
    }

    /// The path of the smallest span that holds `pos`. An empty span holds the position at its start. If no span holds `pos`, the smallest span that ends at `pos`.
    fn at(&self, pos: usize) -> Option<Vec<usize>> {
        let holds =
            |s: &Range<usize>| s.start <= pos && (pos < s.end || s.is_empty() && pos == s.start);
        let smallest = |f: &dyn Fn(&Range<usize>) -> bool| {
            self.0
                .iter()
                .filter(|(_, s)| f(s))
                .min_by_key(|(p, s)| (s.len(), usize::MAX - p.len()))
                .map(|(p, _)| p.clone())
        };
        smallest(&holds).or_else(|| smallest(&|s: &Range<usize>| s.end == pos))
    }
}

/// The binding strength of the top of the text, or the highest if the text does not parse.
fn code_prec(text: &str) -> u8 {
    Expr::parse_lenient(text).map_or(u8::MAX, |e| prec(&e.root))
}

/// The text in parentheses if its top binds less than `min`.
fn wrap_code(text: &str, min: u8) -> String {
    if code_prec(text) < min {
        format!("({text})")
    } else {
        text.to_owned()
    }
}

/// The lenient parse of the text and the span tree, or `None` if the text does not parse.
fn read(text: &str) -> Option<(Node, super::parser::Spans)> {
    parse_spanned(text, true).ok()
}
