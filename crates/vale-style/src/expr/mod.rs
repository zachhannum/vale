//! The expression language of filters, style properties, and label text.
//!
//! `docs/EXPRESSIONS.md` has the grammar and the functions.

mod eval;
mod functions;
mod lexer;
mod parser;

use std::collections::BTreeMap;
use std::fmt;

use vale_render::Color;
use vale_store::GeometryKind;
pub use vale_store::Value;

pub use functions::{FUNCTIONS, Function};
use parser::Node;

/// The attributes of one feature, by field name.
pub trait Attributes {
    fn get(&self, field: &str) -> Option<&Value>;
}

impl Attributes for BTreeMap<String, Value> {
    fn get(&self, field: &str) -> Option<&Value> {
        BTreeMap::get(self, field)
    }
}

/// What an expression can read.
#[derive(Copy, Clone)]
pub struct Context<'a> {
    pub attributes: &'a dyn Attributes,
    /// The map scale denominator. On the globe and the flat view, it is the scale at the center of the view.
    pub scale: f64,
    pub geometry: GeometryKind,
}

/// An error in the text of an expression.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    /// The span of the error, in chars.
    pub start: usize,
    pub end: usize,
    /// From 1.
    pub line: usize,
    /// From 1, in chars.
    pub column: usize,
    pub message: String,
}

impl ParseError {
    fn new(chars: &[char], start: usize, end: usize, message: impl Into<String>) -> Self {
        let before = &chars[..start.min(chars.len())];
        let line = before.iter().filter(|c| **c == '\n').count() + 1;
        let column = before.iter().rev().take_while(|c| **c != '\n').count() + 1;
        ParseError {
            start,
            end: end.max(start),
            line,
            column,
            message: message.into(),
        }
    }

    /// The line of `source` with the error, and a line of carets under the span.
    pub fn pointer(&self, source: &str) -> String {
        let text = source.lines().nth(self.line - 1).unwrap_or("");
        let width = (self.end - self.start).max(1);
        format!(
            "{text}\n{}{}",
            " ".repeat(self.column - 1),
            "^".repeat(width)
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A parsed expression.
pub struct Expr {
    root: Node,
}

impl fmt::Debug for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Expr")
    }
}

impl Expr {
    pub fn parse(source: &str) -> Result<Expr, ParseError> {
        let chars: Vec<char> = source.chars().collect();
        let tokens = lexer::lex(&chars)?;
        let root = parser::Parser::new(&chars, tokens).parse()?;
        Ok(Expr { root })
    }

    pub fn eval(&self, ctx: &Context) -> Value {
        eval::eval(&self.root, ctx)
    }

    /// The names of the fields that the expression reads, sorted and without repeats.
    pub fn fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_fields(&self.root, &mut out);
        out.sort();
        out.dedup();
        out
    }
}

fn collect_fields(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Field(name) => out.push(name.clone()),
        Node::Lit(_) | Node::Var(_) => {}
        Node::Not(a) | Node::Neg(a) | Node::IsNull { value: a, .. } => collect_fields(a, out),
        Node::Binary(_, a, b)
        | Node::Like {
            value: a,
            pattern: b,
            ..
        } => {
            collect_fields(a, out);
            collect_fields(b, out);
        }
        Node::In { value, list, .. } => {
            collect_fields(value, out);
            list.iter().for_each(|n| collect_fields(n, out));
        }
        Node::Call(_, args) => args.iter().for_each(|n| collect_fields(n, out)),
    }
}

/// The text of an expression and the result of its parse. A wrong text stays, so that you can edit it.
#[derive(Debug)]
pub struct Source {
    text: String,
    parsed: Result<Expr, ParseError>,
}

impl Source {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let parsed = Expr::parse(&text);
        Source { text, parsed }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn expr(&self) -> Option<&Expr> {
        self.parsed.as_ref().ok()
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.parsed.as_ref().err()
    }
}

/// A filter of a rule. A blank filter matches each feature, and a wrong filter matches none.
#[derive(Debug)]
pub struct Filter {
    source: Option<Source>,
}

impl Filter {
    pub fn new(text: &str) -> Self {
        let source = (!text.trim().is_empty()).then(|| Source::new(text));
        Filter { source }
    }

    /// The filter that matches each feature.
    pub fn all() -> Self {
        Filter { source: None }
    }

    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.source.as_ref().and_then(Source::error)
    }

    /// True if the result is `true`. Null and all other values do not match.
    pub fn matches(&self, ctx: &Context) -> bool {
        match &self.source {
            None => true,
            Some(s) => s.expr().is_some_and(|e| functions::truthy(&e.eval(ctx))),
        }
    }
}

/// A type that a property can have.
pub trait FromValue: Sized {
    fn from_value(v: &Value) -> Option<Self>;
}

impl FromValue for f64 {
    fn from_value(v: &Value) -> Option<Self> {
        v.as_f64()
    }
}

impl FromValue for bool {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl FromValue for String {
    fn from_value(v: &Value) -> Option<Self> {
        functions::text_of(v)
    }
}

/// A color reads hex text, for example `'#2f6f9f'`.
impl FromValue for Color {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Text(s) => Color::from_hex(s.trim()),
            _ => None,
        }
    }
}

/// A style property: a constant, or an expression with a fallback. The fallback replaces a wrong expression and a result of the wrong type.
#[derive(Debug)]
pub struct Property<T> {
    pub fallback: T,
    source: Option<Source>,
}

impl<T: FromValue + Clone> Property<T> {
    pub fn constant(value: T) -> Self {
        Property {
            fallback: value,
            source: None,
        }
    }

    pub fn expression(text: &str, fallback: T) -> Self {
        Property {
            fallback,
            source: Some(Source::new(text)),
        }
    }

    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.source.as_ref().and_then(Source::error)
    }

    pub fn eval(&self, ctx: &Context) -> T {
        self.source
            .as_ref()
            .and_then(Source::expr)
            .and_then(|e| T::from_value(&e.eval(ctx)))
            .unwrap_or_else(|| self.fallback.clone())
    }
}
