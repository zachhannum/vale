//! Reads the good parts of text that does not parse.

use std::ops::Range;

use super::super::functions;
use super::super::lexer::{self, Tok as Lex, Token};
use super::super::parser::{Node, Spans, Var};
use super::cond::{Cond, cond_of};
use super::seq::{Op, Seq, Tok, seq_of};
use super::template::{Piece, pieces_of};
use super::{SpanMap, read};

/// The tokens of a text, without the end token.
struct Lexed {
    chars: Vec<char>,
    tokens: Vec<Token>,
}

impl Lexed {
    /// `None` if the text does not lex.
    fn new(text: &str) -> Option<Lexed> {
        let chars: Vec<char> = text.chars().collect();
        let mut tokens = lexer::lex(&chars).ok()?;
        tokens.pop();
        Some(Lexed { chars, tokens })
    }

    /// The char span of the tokens `r`. No tokens give an empty span at the place of the next token.
    fn span(&self, r: Range<usize>) -> Range<usize> {
        if r.is_empty() {
            let at = self
                .tokens
                .get(r.start)
                .map_or(self.chars.len(), |t| t.start);
            at..at
        } else {
            self.tokens[r.start].start..self.tokens[r.end - 1].end
        }
    }

    fn text(&self, r: Range<usize>) -> String {
        self.chars[self.span(r)].iter().collect()
    }

    fn is_keyword(&self, i: usize, kw: &str) -> bool {
        matches!(&self.tokens[i].tok, Lex::Name(n) if n.eq_ignore_ascii_case(kw))
    }

    /// The parse of the tokens `r`, with spans in the whole text.
    fn parse(&self, r: Range<usize>) -> Option<(Node, Spans)> {
        let span = self.span(r.clone());
        let (node, mut sp) = read(&self.text(r))?;
        shift(&mut sp, span.start);
        Some((node, sp))
    }

    /// The token after the parenthesis that closes the one at `open`, or `None` if it does not close before `end`.
    fn close(&self, open: usize, end: usize) -> Option<usize> {
        let mut depth = 0;
        for i in open..end {
            match self.tokens[i].tok {
                Lex::LParen => depth += 1,
                Lex::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The parts of the tokens `r` between the separators outside parentheses.
    fn split(&self, r: Range<usize>, mut sep: impl FnMut(usize) -> bool) -> Vec<Range<usize>> {
        let mut parts = Vec::new();
        let mut depth = 0i32;
        let mut start = r.start;
        for i in r.clone() {
            match self.tokens[i].tok {
                Lex::LParen => depth += 1,
                Lex::RParen => depth -= 1,
                _ if depth == 0 && sep(i) => {
                    parts.push(start..i);
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(start..r.end);
        parts
    }
}

fn shift(sp: &mut Spans, by: usize) {
    sp.span = sp.span.start + by..sp.span.end + by;
    sp.kids.iter_mut().for_each(|k| shift(k, by));
}

pub(super) fn template(text: &str, spans: &mut SpanMap) -> Vec<Piece> {
    let len = text.chars().count();
    spans.add(&[], 0..len);
    let mut pieces = Vec::new();
    let Some(lx) = Lexed::new(text) else {
        spans.add(&[0], 0..len);
        pieces.push(Piece::Code(text.to_owned()));
        return pieces;
    };
    let n = lx.tokens.len();
    let mut path = Vec::new();
    let concat = n > 2
        && lx.is_keyword(0, "concat")
        && lx.tokens[1].tok == Lex::LParen
        && lx.close(1, n) == Some(n);
    if concat {
        for part in lx.split(2..n - 1, |i| lx.tokens[i].tok == Lex::Comma) {
            template_part(&lx, part, &mut pieces, &mut path, spans);
        }
        return pieces;
    }
    let parts = lx.split(0..n, |i| lx.tokens[i].tok == Lex::Op("||"));
    if parts.len() < 2 {
        template_part(&lx, 0..n, &mut pieces, &mut path, spans);
        return pieces;
    }
    path.push(0);
    spans.add(&path, 0..len);
    let mut items = Vec::new();
    for part in parts {
        template_part(&lx, part, &mut items, &mut path, spans);
    }
    pieces.push(Piece::Group(items));
    pieces
}

fn template_part(
    lx: &Lexed,
    r: Range<usize>,
    list: &mut Vec<Piece>,
    path: &mut Vec<usize>,
    spans: &mut SpanMap,
) {
    match lx.parse(r.clone()) {
        Some((node, sp)) => pieces_of(&node, &sp, list, path, spans),
        None => {
            path.push(list.len());
            spans.add(path, lx.span(r.clone()));
            path.pop();
            list.push(Piece::Code(lx.text(r)));
        }
    }
}

pub(super) fn condition(text: &str, spans: &mut SpanMap) -> Cond {
    spans.add(&[], 0..text.chars().count());
    match Lexed::new(text) {
        Some(lx) => cond_part(&lx, 0..lx.tokens.len(), &mut Vec::new(), spans),
        None => Cond::Code(text.to_owned()),
    }
}

/// The condition of the tokens `r`. `path` is its path.
fn cond_part(lx: &Lexed, r: Range<usize>, path: &mut Vec<usize>, spans: &mut SpanMap) -> Cond {
    if let Some((node, sp)) = lx.parse(r.clone()) {
        return cond_of(&node, &sp, path, spans);
    }
    if !path.is_empty() {
        spans.add(path, lx.span(r.clone()));
    }
    let ors = lx.split(r.clone(), |i| lx.is_keyword(i, "or"));
    let (all, parts) = if ors.len() > 1 {
        (false, ors)
    } else {
        // The `and` of a `between` does not split.
        let mut between = false;
        let ands = lx.split(r.clone(), |i| {
            if lx.is_keyword(i, "between") {
                between = true;
            }
            let and = lx.is_keyword(i, "and");
            let split = and && !between;
            if and {
                between = false;
            }
            split
        });
        (true, ands)
    };
    if parts.len() < 2 {
        return Cond::Code(lx.text(r));
    }
    let items = parts
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            path.push(i);
            let c = cond_part(lx, part, path, spans);
            path.pop();
            c
        })
        .collect();
    Cond::Group { all, items }
}

pub(super) fn formula(text: &str, spans: &mut SpanMap) -> Seq {
    match Lexed::new(text) {
        Some(lx) => seq_part(&lx, 0..lx.tokens.len(), &mut Vec::new(), spans),
        None => {
            spans.add(&[], 0..text.chars().count());
            spans.add(&[0], 0..text.chars().count());
            Seq(vec![Tok::Code(text.to_owned())])
        }
    }
}

/// The tokens of the token range `r`. `path` is the path of the sequence.
fn seq_part(lx: &Lexed, r: Range<usize>, path: &mut Vec<usize>, spans: &mut SpanMap) -> Seq {
    if let Some((node, sp)) = lx.parse(r.clone()) {
        return seq_of(&node, &sp, path, spans);
    }
    spans.add(path, lx.span(r.clone()));
    let mut toks = Vec::new();
    let mut i = r.start;
    while i < r.end {
        path.push(toks.len());
        let (tok, next) = tok_at(lx, i, r.end, path, spans);
        spans.add(path, lx.span(i..next));
        path.pop();
        toks.push(tok);
        i = next;
    }
    Seq(toks)
}

/// The token that starts at token `i`, and the index after it. `path` is the path of the token.
fn tok_at(
    lx: &Lexed,
    i: usize,
    end: usize,
    path: &mut Vec<usize>,
    spans: &mut SpanMap,
) -> (Tok, usize) {
    let next = lx.tokens.get(i + 1).filter(|_| i + 1 < end).map(|t| &t.tok);
    let tok = match &lx.tokens[i].tok {
        Lex::Number(n) => Tok::Num(*n),
        Lex::Str(s) => Tok::Text(s.clone()),
        Lex::Quoted(name) => Tok::Field(name.clone()),
        Lex::Name(name) if next == Some(&Lex::LParen) => {
            let Some(close) = lx.close(i + 1, end) else {
                return (Tok::Code(lx.text(i..end)), end);
            };
            let Some(f) = functions::find(name) else {
                return (Tok::Code(lx.text(i..close)), close);
            };
            let args = lx
                .split(i + 2..close - 1, |k| lx.tokens[k].tok == Lex::Comma)
                .into_iter()
                .enumerate()
                .map(|(k, part)| {
                    path.push(k);
                    let seq = seq_part(lx, part, path, spans);
                    path.pop();
                    seq
                })
                .collect();
            return (Tok::Call { f, args }, close);
        }
        Lex::Name(name) => match name.to_ascii_lowercase().as_str() {
            "true" => Tok::Bool(true),
            "false" => Tok::Bool(false),
            "and" => Tok::Op(Op::And),
            "or" => Tok::Op(Op::Or),
            "not" => Tok::Op(Op::Not),
            kw if super::super::is_keyword(kw) => Tok::Code(name.clone()),
            _ => Tok::Field(name.clone()),
        },
        Lex::LParen => {
            let Some(close) = lx.close(i, end) else {
                return (Tok::Code(lx.text(i..end)), end);
            };
            path.push(0);
            let seq = seq_part(lx, i + 1..close - 1, path, spans);
            path.pop();
            return (Tok::Paren(seq), close);
        }
        Lex::Op(op) => match *op {
            "=" => Tok::Op(Op::Eq),
            "!=" | "<>" => Tok::Op(Op::Ne),
            "<" => Tok::Op(Op::Lt),
            "<=" => Tok::Op(Op::Le),
            ">" => Tok::Op(Op::Gt),
            ">=" => Tok::Op(Op::Ge),
            "||" => Tok::Op(Op::Concat),
            "+" => Tok::Op(Op::Add),
            "-" => Tok::Op(Op::Sub),
            "*" => Tok::Op(Op::Mul),
            "/" => Tok::Op(Op::Div),
            "%" => Tok::Op(Op::Rem),
            _ => Tok::Code(lx.text(i..i + 1)),
        },
        Lex::Var(name) => match Var::ALL
            .into_iter()
            .find(|v| v.name().eq_ignore_ascii_case(name))
        {
            Some(v) => Tok::Var(v),
            None => Tok::Code(lx.text(i..i + 1)),
        },
        Lex::RParen | Lex::Comma | Lex::End => Tok::Code(lx.text(i..i + 1)),
    };
    (tok, i + 1)
}
